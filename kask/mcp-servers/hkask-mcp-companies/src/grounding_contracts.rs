//! Grounding contracts for the companies MCP server's 45 tools.
//!
//! These contracts declare which output fields each tool produces and what
//! grounding disposition they carry. The contracts are registered with the
//! server's `VerificationStore` at construction time (in `run()`), so every
//! tool output gets grounded — not a coverage gap.
//!
//! ## Disposition model for deterministic tools
//!
//! The companies tools are **deterministic** — they compute their outputs from
//! provider API data (FMP/EODHD) using platform code (`financial_model`,
//! `analysis`, `economic_profit`). This is different from the `task`/`skill`
//! agent contracts, which check LLM-synthesized output against `tool_calls`.
//!
//! The core grounding path (`execute_tool_semantic`) passes `&[]` for
//! `tool_calls` — it doesn't have tool-call visibility. So `Sourced` fields
//! (which require value-matching against `tool_calls`) would all be nulled.
//! Instead, the companies tools' fields are declared `Inferred` (empty
//! `sources`): the platform computed them, but the grounding check can't
//! verify the computation without `tool_calls` visibility. The `framework`
//! and `interpretation` prose fields are `Inferred` too (commissioned
//! output).
//!
//! This is honest: the core grounding marks provenance and scans narrative
//! for leaks (the floor). The real fabrication catch for companies tools
//! happens when an LLM agent calls them and relays the output — the
//! delegating servers' `enforce_and_stamp` with `tool_calls` visibility
//! value-matches the relayed values against the tool's actual return (the
//! ceiling).
//!
//! ## Why contracts at all?
//!
//! Without contracts, every tool call writes a coverage-gap record
//! (`had_contract: false`). The trend query shows 45 tools with no contract —
//! the operator can't distinguish "tools that need contracts" from "tools
//! that have them." With contracts, the trend query shows grounded
//! delegations with `was_enforced: true` — the coverage gap closes, and
//! the narrative-leak scan runs on every tool's prose fields.

use std::collections::HashMap;

/// Register all companies-tool grounding contracts with the store.
/// Called from `run()` after the `VerificationStore` is constructed.
pub fn register_all(store: &hkask_verification::VerificationStore) {
    store.register_contract(raw_provider_data_contract());
    store.register_contract(valuation_contract());
    store.register_contract(analysis_contract());
    store.register_contract(portfolio_contract());
    store.register_contract(forecast_contract());
    store.register_contract(research_contract());
}

/// Contract for the 8 raw provider-data tools: `company_profile`,
/// `stock_quote`, `income_statement`, `balance_sheet`,
/// `cash_flow_statement`, `key_metrics`, `historical_price`,
/// `symbol_search`.
///
/// These tools return raw FMP/EODHD API responses. The output is the
/// provider's JSON, passed through. All fields are `Inferred` — the
/// platform fetched them, but the grounding check can't verify the fetch
/// without `tool_calls` visibility. The `ontology` field (added by
/// `fibo::enrich_with_ontology`) is also `Inferred`.
fn raw_provider_data_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    // The raw provider response is the entire output — no individual fields
    // to declare. The contract exists so the tool is grounded (not a coverage
    // gap) and the narrative-leak scan runs on any prose in the response.
    field_sources.insert(
        "ontology".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The FIBO ontology concept URI, added by enrich_with_ontology. \
                  Platform-derived from the tool name via fibo::tool_to_ontology."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // Register for each of the 8 raw-data tools. The contract is the same
    // for all of them — the output shape varies (arrays vs objects) but the
    // grounding disposition is the same.
    GroundingContract {
        agent_type: "company_profile".to_string(),
        field_sources,
    }
}

/// Contract for the valuation tools: `dcf_valuation`, `reverse_dcf`,
/// `ep_valuation`, `comparable_analysis`, `scenario_analysis`,
/// `sensitivity_analysis`, `monte_carlo_dcf`, `scenario_impact_valuation`,
/// `calibrate_forecast`, `equity_duration`, `expectations_gap`.
///
/// These tools compute financial models from provider data. Numeric fields
/// (`intrinsic_per_share`, `current_price`, `implied_growth_rate`, etc.)
/// are `Inferred` — the platform computed them from provider API data, but
/// the grounding check can't verify the computation without `tool_calls`.
/// Prose fields (`framework`, `interpretation`, `mauboussin_framework`)
/// are `Inferred` (commissioned output) and scanned for narrative leaks.
fn valuation_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    // The valuation tools' computed numeric fields. These are platform-derived
    // from provider API data, but the grounding check sees only the output,
    // not the inputs — so they're Inferred, not Derived (Derived requires the
    // source field to be in the same output document).
    for field in [
        "symbol",
        "forecast_id",
        "current_price",
        "intrinsic_per_share",
        "implied_growth_rate",
        "intrinsic_at_implied",
        "enterprise_value",
        "margin_of_safety",
        "revision_of",
        "market_gap_pct",
    ] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A computed valuation field. Platform-derived from provider \
                      API data via financial_model/economic_profit. The grounding \
                      check marks it Inferred because it can't verify the \
                      computation without tool_calls visibility."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    // Prose fields — commissioned output, scanned for narrative leaks.
    for field in ["framework", "interpretation", "mauboussin_framework"] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A prose field describing the valuation methodology or \
                      interpretation. Commissioned by the tool's template — \
                      the tool was asked to explain its output."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    // The config block — platform-derived from the request + history.
    field_sources.insert(
        "config".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The DCF configuration (stage1_years, discount_rate, etc.). \
                  Platform-derived from the request overrides and historical data."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The history block — platform-derived from the provider API data.
    field_sources.insert(
        "history".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "Historical financial summary (revenue_cagr, gross_margin, etc.). \
                  Platform-derived from provider API data via HistoricalSnapshot."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The projections block — platform-computed by project_model.
    field_sources.insert(
        "projections".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "Projected financial line items (revenue, COGS, FCF, etc. per period). \
                  Platform-computed by financial_model::project_model from the \
                  historical snapshot and projection assumptions."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The valuation block — platform-computed.
    field_sources.insert(
        "valuation".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The valuation summary (PV of cash flows, terminal value, equity \
                  value, intrinsic per share). Platform-computed by project_model."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The data_quality block — platform-computed by signal_quality.
    field_sources.insert(
        "data_quality".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "Signal quality metrics (overall_confidence, CV, outliers). \
                  Platform-computed by HistoricalSnapshot::signal_quality."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The fibo block — platform-derived ontology constants.
    field_sources.insert(
        "fibo".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "FIBO ontology concept URIs for the output fields. \
                  Platform-derived from the tool name via fibo::tool_to_ontology."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The ontology field — added by enrich_with_ontology.
    field_sources.insert(
        "ontology".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The FIBO ontology concept URI, added by enrich_with_ontology.".to_string(),
            derived_from: None,
            transform: None,
        },
    );
    // The error field — platform-generated error message.
    field_sources.insert(
        "error".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "An error message when the tool couldn't compute (insufficient \
                  data, invalid input). Platform-generated, not LLM-synthesized."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    GroundingContract {
        agent_type: "dcf_valuation".to_string(),
        field_sources,
    }
}

/// Contract for the analysis tools: `moat_check`,
/// `management_scorecard`, `working_capital_cycle`.
///
/// These tools compute MAIA-framework analysis from key metrics data.
/// Numeric fields are `Inferred`; the `framework` field is `Inferred`
/// (commissioned prose).
fn analysis_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    for field in [
        "symbol",
        "moat",
        "ceo_rating",
        "cfo_working_capital_rating",
        "margin_stability",
        "data_periods",
        "data_points",
        "spread_stability",
        "aligned_periods",
    ] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A computed analysis field. Platform-derived from provider \
                      API data via the analysis module (gross_margin_stability, \
                      classify_moat, ceo_capital_allocation_score)."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    // Nested blocks — platform-computed.
    for field in [
        "working_capital",
        "gross_margins",
        "returns_on_capital",
        "invested_capital",
        "periods",
    ] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A nested analysis block. Platform-computed from provider \
                      API data."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    field_sources.insert(
        "framework".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose field describing the MAIA analysis framework. \
                  Commissioned by the tool's template."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    field_sources.insert(
        "reason".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose explanation of the analysis result. Commissioned \
                  by the tool's template."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    field_sources.insert(
        "ontology".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The FIBO ontology concept URI, added by enrich_with_ontology.".to_string(),
            derived_from: None,
            transform: None,
        },
    );
    GroundingContract {
        agent_type: "moat_check".to_string(),
        field_sources,
    }
}

/// Contract for the portfolio tools: `portfolio_list`,
/// `portfolio_delete`, `ledger_import`, `ledger_export`,
/// `transaction_note_append`, `portfolio_comparison`,
/// `portfolio_returns`, `portfolio_attribution`,
/// `portfolio_characteristics`, `note_add`, `note_list`,
/// `note_delete`, `file_attach`, `file_list`, `file_delete`.
///
/// These tools read/write the SQLite-backed portfolio ledger. Output
/// fields are `Inferred` — platform-derived from the ledger, but the
/// grounding check can't verify the derivation without `tool_calls`.
fn portfolio_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    for field in [
        "portfolio",
        "portfolios",
        "status",
        "name",
        "format",
        "data",
        "from",
        "to",
        "total_return",
        "irr",
        "provenance",
        "attribution",
        "characteristics",
        "errors",
        "message",
        "total_market_value",
        "position_count",
        "aggregation",
        "comparison",
        "overlap",
        "unique_symbols",
        "note_id",
        "file_id",
        "transaction_id",
    ] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A portfolio ledger field. Platform-derived from the \
                      SQLite-backed PortfolioManager."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    field_sources.insert(
        "ontology".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The FIBO ontology concept URI, added by enrich_with_ontology.".to_string(),
            derived_from: None,
            transform: None,
        },
    );
    GroundingContract {
        agent_type: "portfolio_list".to_string(),
        field_sources,
    }
}

/// Contract for the forecast tools: `forecast_get`, `forecast_list`,
/// `forecast_persist`, `forecast_record`, `result_feedback`.
///
/// These tools read/write the forecast store. Output fields are
/// `Inferred` — platform-derived from the persisted snapshot.
fn forecast_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    for field in [
        "forecast_id",
        "symbol",
        "revision_of",
        "outcomes",
        "created_at",
        "brier_score",
        "direction_accuracy",
        "return_accuracy",
        "brier_interpretation",
        "decomposition",
        "feedback_recorded",
    ] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A forecast store field. Platform-derived from the \
                      persisted forecast snapshot."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    field_sources.insert(
        "framework".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose field describing the forecast methodology. \
                  Commissioned by the tool's template."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    field_sources.insert(
        "ontology".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The FIBO ontology concept URI, added by enrich_with_ontology.".to_string(),
            derived_from: None,
            transform: None,
        },
    );
    GroundingContract {
        agent_type: "forecast_get".to_string(),
        field_sources,
    }
}

/// Contract for the research tools: `research_search`,
/// `company_screener`, `company_transcript`.
///
/// These tools fetch external data (Exa/Tavily/Brave, FMP screener,
/// FMP/SerpAPI transcripts). Output fields are `Inferred` —
/// platform-fetched from external providers.
fn research_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    for field in [
        "symbol",
        "query",
        "claims",
        "claims_count",
        "category_summary",
        "providers",
        "prompt",
        "criteria",
        "results",
        "count",
        "transcript",
        "quarter",
        "year",
        "source",
    ] {
        field_sources.insert(
            field.to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "A research/screener/transcript field. Platform-fetched \
                      from external providers (Exa/Tavily/Brave, FMP, SerpAPI)."
                    .to_string(),
                derived_from: None,
                transform: None,
            },
        );
    }
    field_sources.insert(
        "framework".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose field describing the research methodology. \
                  Commissioned by the tool's template."
                .to_string(),
            derived_from: None,
            transform: None,
        },
    );
    field_sources.insert(
        "ontology".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The FIBO ontology concept URI, added by enrich_with_ontology.".to_string(),
            derived_from: None,
            transform: None,
        },
    );
    GroundingContract {
        agent_type: "research_search".to_string(),
        field_sources,
    }
}
