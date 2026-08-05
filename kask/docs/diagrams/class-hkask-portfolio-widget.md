---
title: "hKask Portfolio Widget — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Composition"
mds_categories: [composition]
---

# hKask Portfolio Widget — Class Diagram

`hkask-portfolio-widget` renders ```` ```portfolio ```` fenced blocks as a
portfolio dashboard (summary tiles → returns detail → characteristics table →
attribution ranking). It is a passive renderer: the curator agent calls the
`companies` MCP tools (`portfolio_list`, `portfolio_returns`,
`portfolio_characteristics`, `portfolio_attribution`) and emits the combined
result as the block body. The portfolio selector / aggregation controls /
auto-loader / comparison mode from the deleted standalone
`PortfolioDashboardView` are intentionally omitted — the agent picks the
portfolio and the body already carries its data.

```mermaid
classDiagram
    class PortfolioBlockBody {
        +viz: Option~String~
        +portfolio: Option~String~
        +returns: Option~ReturnsBody~
        +characteristics: HashMap~String, CharacteristicField~
        +attribution: Vec~AttributionRow~
    }
    class ReturnsBody {
        +portfolio: Option~String~
        +from: Option~String~
        +to: Option~String~
        +total_return: f64
        +modified_dietz: f64
        +irr: f64
        +irr_converged: bool
        +start_value: f64
        +end_value: f64
        +net_cash_flows: f64
        +cash_flow_count: usize
        +positions_at_start: usize
        +positions_at_end: usize
    }
    class CharacteristicField {
        +value: Option~f64~
        +fibo: Option~String~
        +method: Option~String~
        +holdings: Option~usize~
    }
    class AttributionRow {
        +symbol: String
        +weight_start_pct: f64
        +weight_end_pct: f64
        +security_return_pct: f64
        +contribution_bps: f64
        +gain_loss: f64
    }
    class PortfolioWidget {
        +body: PortfolioBlockBody
        +focus_handle: FocusHandle
        +new(body, cx) PortfolioWidget
        +render_summary_tiles()
        +render_returns_detail()
        +render_characteristics()
        +render_attribution()
    }
    class create_portfolio_widget {
        +create_portfolio_widget(body, cx) Option~Entity~PortfolioWidget~~
    }

    PortfolioBlockBody "1" o-- "0..1" ReturnsBody : returns
    PortfolioBlockBody "1" o-- "many" CharacteristicField : characteristics
    PortfolioBlockBody "1" o-- "many" AttributionRow : attribution
    PortfolioWidget --> PortfolioBlockBody
    PortfolioWidget ..|> gpui_Focusable : Focusable
    PortfolioWidget ..|> gpui_Render : Render
    create_portfolio_widget ..> PortfolioWidget : viz is portfolio
```

**Block shape:** a JSON body with `viz: "portfolio"`. `returns`,
`characteristics`, and `attribution` are all optional so partial bodies still
render the present sections. `AttributionRow.symbol` is required (a row without
a symbol is meaningless); other attribution fields default.

**FIBO anchors:** displayed metrics are anchored to the FIBO ontology via
constants (`FIBO_TIME_WEIGHTED_RETURN`, `FIBO_INTERNAL_RATE_OF_RETURN`,
`FIBO_TRANSACTION_LEDGER`, …) that match the `fibo` map entries the
`companies` MCP server includes in its responses.

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-PORTFOLIO
verified_date: 2026-08-04
verified_against: crates/hkask-portfolio-widget/src/block.rs; crates/hkask-portfolio-widget/src/view.rs
status: VERIFIED
-->