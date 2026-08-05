//! The `PortfolioWidget` GPUI view — renders the portfolio dashboard inline in
//! agent markdown (via the D18 seam composed by `hkask-viz-core`).
//!
//! Data comes from the parsed `PortfolioBlockBody` (emitted by the curator
//! agent after calling the `companies` MCP tools). The portfolio selector,
//! aggregation controls, auto-loader, and comparison mode from the deleted
//! standalone `PortfolioDashboardView` are intentionally omitted — the agent
//! picks the portfolio and the block body already contains the data for it.
//!
//! T5 (widget sovereignty): the returns detail carries a date-range scrub
//! affordance. The user types a new `from`/`to` and clicks Apply; the widget
//! re-issues `portfolio_returns` via the governed `shared_tool_invoker()`
//! (OCAP/gas-budgeted in production via `McpRuntime`), merging the new dates
//! into the block's server-authoritative provenance args. A missing invoker
//! and non-dispatchable provenance are surfaced as visible states, never
//! silent no-ops (repo `.rules`).
//!
//! Renders the same dashboard layout: header → summary tiles (total return,
//! IRR, modified Dietz, start/end value, positions) → returns detail →
//! characteristics table → attribution ranking.

use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, KeyDownEvent, Keystroke, Render, Window,
    prelude::*,
};
use gpui_util::ResultExt as _;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use ui::prelude::*;

use crate::block::{
    AttributionRow, CharacteristicField, FIBO_INTERNAL_RATE_OF_RETURN, FIBO_TIME_WEIGHTED_RETURN,
    FIBO_TRANSACTION_LEDGER, PortfolioBlockBody,
};

/// Server that hosts the portfolio tools. Fallback dispatch target when a
/// block carries no dispatchable provenance.
const DEFAULT_SERVER: &str = "hkask-mcp-companies";
/// Tool the widget re-issues to scrub a date range.
const DEFAULT_TOOL: &str = "portfolio_returns";
/// Surfaced when the process-global `ToolInvoker` is not wired. Visible state,
/// not a silent no-op (repo `.rules` startup-failure-signal trap).
const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";
/// Surfaced when provenance is partial (non-dispatchable but not empty): the
/// widget refuses to re-issue the wrong tool and asks the user to route through
/// the agent.
const PROVENANCE_INCOMPLETE_MSG: &str = "provenance incomplete — ask the agent";
/// Surfaced when the scrubbed dates are not `YYYY-MM-DD`.
const DATE_FORMAT_ERR: &str = "from/to must be YYYY-MM-DD";

/// The portfolio widget view. Renders inline in agent markdown (via the D18
/// seam composed by `hkask-viz-core`).
pub struct PortfolioWidget {
    body: PortfolioBlockBody,
    focus_handle: FocusHandle,
    /// Focus handles for the two editable date chips (T5 scrub affordance).
    from_focus: FocusHandle,
    to_focus: FocusHandle,
    /// Editable scrub target dates (`YYYY-MM-DD`), seeded from the block's
    /// returns. The user types into the focused chip; Apply dispatches.
    from_input: String,
    to_input: String,
    /// Tool name currently being dispatched, if a scrub is in flight.
    dispatch_in_flight: Option<String>,
    /// Visible error/hint when dispatch cannot proceed (missing invoker,
    /// provenance incomplete, malformed date, tool error). Never silently
    /// dropped (repo `.rules`).
    dispatch_error: Option<String>,
}

impl PortfolioWidget {
    /// Create a new portfolio widget for the parsed block body.
    pub fn new(body: PortfolioBlockBody, cx: &mut Context<Self>) -> Self {
        hkask_tool_invoker::record_render(
            body.provenance.tool.clone(),
            body.provenance.span_id.clone(),
        );
        tracing::info!(
            target: "reg.widget.render",
            tool = body.provenance.tool.as_deref().unwrap_or(""),
            span_id = body.provenance.span_id.as_deref().unwrap_or(""),
            "REG",
        );
        let from_input = body
            .returns
            .as_ref()
            .and_then(|returns| returns.from.clone())
            .unwrap_or_default();
        let to_input = body
            .returns
            .as_ref()
            .and_then(|returns| returns.to.clone())
            .unwrap_or_default();
        Self {
            body,
            focus_handle: cx.focus_handle(),
            from_focus: cx.focus_handle(),
            to_focus: cx.focus_handle(),
            from_input,
            to_input,
            dispatch_in_flight: None,
            dispatch_error: None,
        }
    }

    fn render_summary_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(returns) = &self.body.returns else {
            return Vec::new();
        };

        let border_color = cx.theme().colors().border;

        let tiles = [
            (
                "Total Return",
                format_pct(returns.total_return),
                Some(FIBO_TIME_WEIGHTED_RETURN),
            ),
            (
                "IRR",
                format_pct(returns.irr),
                Some(FIBO_INTERNAL_RATE_OF_RETURN),
            ),
            ("Modified Dietz", format_pct(returns.modified_dietz), None),
            ("Start Value", format_currency(returns.start_value), None),
            ("End Value", format_currency(returns.end_value), None),
            (
                "Positions",
                format!(
                    "{} → {}",
                    returns.positions_at_start, returns.positions_at_end
                ),
                None,
            ),
        ];

        tiles
            .into_iter()
            .map(|(label, value, fibo_concept)| {
                v_flex()
                    .p_3()
                    .gap_1()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(label)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(Label::new(value).size(LabelSize::Large))
                    .when_some(fibo_concept, |this, concept| {
                        this.child(
                            Label::new(concept)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    })
                    .into_any_element()
            })
            .collect()
    }

    fn render_returns_detail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let returns = self.body.returns.as_ref()?;
        let border_color = cx.theme().colors().border;

        let from = returns.from.clone().unwrap_or_else(|| "—".to_string());
        let to = returns.to.clone().unwrap_or_else(|| "—".to_string());

        Some(
            div()
                .p_3()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(border_color)
                .child(
                    Label::new(format!("Returns: {from} to {to}"))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            Label::new(format!(
                                "Net cash flows: {}",
                                format_currency(returns.net_cash_flows)
                            ))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                        )
                        .child(
                            Label::new(format!("Cash flow events: {}", returns.cash_flow_count))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            Label::new(format!(
                                "IRR converged: {}",
                                if returns.irr_converged { "yes" } else { "no" }
                            ))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                        ),
                )
                // T5: date-range scrub affordance (or a disabled "ask the
                // agent" hint when provenance is partial / non-dispatchable).
                .child(self.render_scrub_affordance(cx))
                .when_some(self.render_dispatch_status(cx), |this, status| {
                    this.child(status)
                })
                .into_any_element(),
        )
    }

    fn render_characteristics(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.body.characteristics.is_empty() {
            return div().into_any_element();
        }

        let border_color = cx.theme().colors().border;

        // Sort by field name for stable, deterministic rendering (HashMap
        // iteration order is randomized).
        let mut fields: Vec<(&String, &CharacteristicField)> =
            self.body.characteristics.iter().collect();
        fields.sort_by_key(|(left, _)| *left);

        let rows: Vec<AnyElement> = fields
            .into_iter()
            .map(|(field, characteristic)| {
                let value = characteristic
                    .value
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "—".to_string());
                let fibo = characteristic.fibo.clone().unwrap_or_default();
                let holdings = characteristic.holdings.unwrap_or(0);
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Label::new(field.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(Label::new(value).size(LabelSize::XSmall))
                            .child(
                                Label::new(format!("({holdings} holdings)"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .when(!fibo.is_empty(), |this| {
                        this.child(Label::new(fibo).size(LabelSize::XSmall).color(Color::Muted))
                    })
                    .into_any_element()
            })
            .collect();

        div()
            .p_3()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new("Characteristics")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(rows)
            .into_any_element()
    }

    fn render_attribution(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.body.attribution.is_empty() {
            return div().into_any_element();
        }

        let border_color = cx.theme().colors().border;

        let rows: Vec<AnyElement> = self
            .body
            .attribution
            .iter()
            .map(|row| attribution_row_element(row))
            .collect();

        div()
            .p_3()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!(
                    "Attribution ({} holdings)",
                    self.body.attribution.len()
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .children(rows)
            .into_any_element()
    }

    /// The T5 date-range scrub affordance. When provenance is dispatchable (or
    /// empty — the fallback path), renders two editable date chips plus an
    /// Apply control. When provenance is partial / non-dispatchable, renders a
    /// disabled "ask the agent" hint instead — a visible state, never a silent
    /// no-op (repo `.rules`).
    fn render_scrub_affordance(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;

        if !scrub_enabled(&self.body.provenance) {
            return div()
                .child(
                    Label::new(PROVENANCE_INCOMPLETE_MSG)
                        .size(LabelSize::XSmall)
                        .color(Color::Warning),
                )
                .into_any_element();
        }

        let from_display: String = if self.from_input.is_empty() {
            "YYYY-MM-DD".to_string()
        } else {
            self.from_input.clone()
        };
        let to_display: String = if self.to_input.is_empty() {
            "YYYY-MM-DD".to_string()
        } else {
            self.to_input.clone()
        };

        h_flex()
            .gap_2()
            .child(
                Label::new("Change range:")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .child(
                div()
                    .id("portfolio-scrub-from")
                    .track_focus(&self.from_focus)
                    .rounded_sm()
                    .border_1()
                    .border_color(border_color)
                    .px_2()
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.from_focus.focus(window, cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                        handle_date_keystroke(&mut this.from_input, &event.keystroke);
                        cx.notify();
                    }))
                    .child(Label::new(from_display).size(LabelSize::XSmall)),
            )
            .child(Label::new("→").size(LabelSize::XSmall).color(Color::Muted))
            .child(
                div()
                    .id("portfolio-scrub-to")
                    .track_focus(&self.to_focus)
                    .rounded_sm()
                    .border_1()
                    .border_color(border_color)
                    .px_2()
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.to_focus.focus(window, cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                        handle_date_keystroke(&mut this.to_input, &event.keystroke);
                        cx.notify();
                    }))
                    .child(Label::new(to_display).size(LabelSize::XSmall)),
            )
            .child(
                div()
                    .id("portfolio-scrub-apply")
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.dispatch_change_range(cx);
                    }))
                    .child(
                        Label::new("Apply")
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    ),
            )
            .into_any_element()
    }

    /// Visible dispatch status row: pending spinner label or the surfaced
    /// error/hint. Emits nothing when idle so the widget stays compact.
    fn render_dispatch_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(tool) = &self.dispatch_in_flight {
            return Some(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Dispatching /{tool} …"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.dispatch_error {
            let border_color = cx.theme().colors().border;
            return Some(
                div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(error.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                    .into_any_element(),
            );
        }
        None
    }

    /// Build the dispatch plan from the block's provenance and the scrubbed
    /// dates, then route through the governed `shared_tool_invoker()`
    /// (OCAP/gas-budgeted in production via `McpRuntime`).
    ///
    /// Surfaced states (never silent per repo `.rules`):
    /// - `DATE_FORMAT_ERR` / `PROVENANCE_INCOMPLETE_MSG` when the pure planner
    ///   rejects the request.
    /// - `INVOKER_NOT_WIRED_MSG` when `shared_tool_invoker()` returns `None`.
    /// - The tool's own error string when dispatch fails.
    fn dispatch_change_range(&mut self, cx: &mut Context<Self>) {
        let plan =
            build_returns_dispatch_args(&self.body.provenance, &self.from_input, &self.to_input);
        let (server, tool, args) = match plan {
            Ok(plan) => plan,
            Err(message) => {
                self.dispatch_error = Some(message.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
        };

        let invoker = match shared_tool_invoker() {
            None => {
                self.dispatch_error = Some(INVOKER_NOT_WIRED_MSG.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
            Some(invoker) => invoker,
        };

        self.dispatch_error = None;
        self.dispatch_in_flight = Some(tool.clone());
        let task = invoker.invoke_tool(&server, &tool, args);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                this.dispatch_in_flight = None;
                match outcome {
                    // The conversation is the durable record; the widget only
                    // surfaces in-flight + error states (T5 spec).
                    Ok(_) => this.dispatch_error = None,
                    Err(error) => this.dispatch_error = Some(error),
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }
}

fn attribution_row_element(row: &AttributionRow) -> AnyElement {
    let color = if row.contribution_bps >= 0.0 {
        Color::Created
    } else {
        Color::Error
    };
    h_flex()
        .gap_2()
        .child(Label::new(row.symbol.clone()).size(LabelSize::XSmall))
        .child(
            Label::new(format!("{:+.0} bps", row.contribution_bps))
                .size(LabelSize::XSmall)
                .color(color),
        )
        .child(
            Label::new(format!("{:+.2}%", row.security_return_pct))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .child(
            Label::new(format!("{:.1}% wgt", row.weight_start_pct))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .into_any_element()
}

impl Focusable for PortfolioWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PortfolioWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            // Header
            .child(
                h_flex()
                    .gap_2()
                    .child(Label::new("Portfolio Dashboard").size(LabelSize::Large))
                    .child(
                        Label::new(FIBO_TRANSACTION_LEDGER)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .when_some(self.body.portfolio.as_ref(), |this, name| {
                        this.child(
                            Label::new(format!("· {name}"))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
            // Summary tiles
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(self.render_summary_tiles(cx)),
            )
            // Returns detail
            .when_some(self.render_returns_detail(cx), |this, detail| {
                this.child(detail)
            })
            // Characteristics table
            .child(self.render_characteristics(cx))
            // Attribution ranking
            .child(self.render_attribution(cx))
            .when(
                self.body.returns.is_none()
                    && self.body.characteristics.is_empty()
                    && self.body.attribution.is_empty(),
                |this| {
                    this.child(
                        div()
                            .p_2()
                            .rounded_sm()
                            .border_1()
                            .border_color(border_color)
                            .child(
                                Label::new("No portfolio data in this block.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                },
            )
            .into_any_element()
    }
}

// ── formatting helpers (mirror the deleted view) ────────────────────────

fn format_pct(value: f64) -> String {
    if value.is_finite() {
        format!("{:+.2}%", value * 100.0)
    } else {
        "—".to_string()
    }
}

fn format_currency(value: f64) -> String {
    if value.is_finite() && value.abs() > 0.0 {
        format!("${:.0}", value)
    } else {
        "—".to_string()
    }
}

// ── Pure dispatch-planning logic (T5) ──────────────────────────────────
//
// Kept free of the GPUI executor / global state so the dispatch decision is
// unit-testable directly (repo `.rules` racy-global trap: never unit-test by
// mutating `set_tool_invoker`). The `dispatch_change_range` handler composes
// this pure function with `shared_tool_invoker()`.

/// Whether the scrub affordance is enabled: dispatchable provenance re-issues
/// the originating tool; empty provenance falls back to the hardcoded default.
/// Partial (non-dispatchable, non-empty) provenance is disabled with a hint.
fn scrub_enabled(provenance: &BlockProvenance) -> bool {
    provenance.is_dispatchable() || provenance.is_empty()
}

/// Structural `YYYY-MM-DD` check (4-2-2 digits, dash-separated). The MCP server
/// does the authoritative parse; this keeps the pure helper chrono-free so it
/// is testable without a date-time dependency.
fn is_valid_ymd(value: &str) -> bool {
    let bytes = value.as_bytes();
    // length check first so the byte indexes below are in bounds.
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(|byte| byte.is_ascii_digit())
        && bytes[5..7].iter().all(|byte| byte.is_ascii_digit())
        && bytes[8..10].iter().all(|byte| byte.is_ascii_digit())
}

/// Merge `override_obj` into `base` (both objects). Non-object inputs are
/// treated as empty objects so a `null`/absent `args` still merges cleanly — a
/// block produced before args were recorded re-issues with just the override.
fn merge_args(base: &serde_json::Value, override_obj: &serde_json::Value) -> serde_json::Value {
    let mut merged = serde_json::Map::new();
    if let serde_json::Value::Object(map) = base {
        for (key, value) in map {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let serde_json::Value::Object(overrides) = override_obj {
        for (key, value) in overrides {
            merged.insert(key.clone(), value.clone());
        }
    }
    serde_json::Value::Object(merged)
}

/// Pure: decide the `(server, tool, args)` dispatch tuple for a scrub, given
/// the block's provenance and the user's new `from`/`to`.
///
/// - invalid date format → `Err(DATE_FORMAT_ERR)`.
/// - provenance dispatchable → re-issue `provenance.server` / `provenance.tool`
///   with `{from, to}` merged into a clone of `provenance.args`.
/// - empty provenance → fall back to the hardcoded dispatch:
///   `(DEFAULT_SERVER, DEFAULT_TOOL, {from, to})`.
/// - partial (non-dispatchable, non-empty) provenance →
///   `Err(PROVENANCE_INCOMPLETE_MSG)`.
fn build_returns_dispatch_args(
    provenance: &BlockProvenance,
    new_from: &str,
    new_to: &str,
) -> Result<(String, String, serde_json::Value), &'static str> {
    if !is_valid_ymd(new_from) || !is_valid_ymd(new_to) {
        return Err(DATE_FORMAT_ERR);
    }
    let override_obj = serde_json::json!({ "from": new_from, "to": new_to });
    if provenance.is_dispatchable() {
        // `is_dispatchable()` guarantees tool/server are Some; the
        // `unwrap_or_default` accessors only return an empty string if the
        // invariant were violated, keeping this panic-free.
        let tool = provenance.tool.as_deref().unwrap_or_default().to_string();
        let server = provenance.server.as_deref().unwrap_or_default().to_string();
        let merged = merge_args(&provenance.args, &override_obj);
        Ok((server, tool, merged))
    } else if provenance.is_empty() {
        Ok((
            DEFAULT_SERVER.to_string(),
            DEFAULT_TOOL.to_string(),
            override_obj,
        ))
    } else {
        Err(PROVENANCE_INCOMPLETE_MSG)
    }
}

/// Minimal single-line text input handler for a date chip. Appends typed digit
/// / `-` characters (capped at 10 — the length of `YYYY-MM-DD`) and pops on
/// backspace. Other keystrokes (modifiers, non-digit chars) are ignored so the
/// chip only accepts well-formed date input.
fn handle_date_keystroke(buffer: &mut String, keystroke: &Keystroke) {
    match keystroke.key.as_str() {
        "backspace" => {
            buffer.pop();
        }
        _ => {
            if let Some(typed) = keystroke.key_char.as_deref()
                && typed.len() == 1
            {
                let glyph = typed.chars().next().unwrap_or_default();
                if (glyph.is_ascii_digit() || glyph == '-') && buffer.len() < 10 {
                    buffer.push(glyph);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pct_positive() {
        assert_eq!(format_pct(0.15), "+15.00%");
    }

    #[test]
    fn format_pct_negative() {
        assert_eq!(format_pct(-0.05), "-5.00%");
    }

    #[test]
    fn format_pct_nan() {
        assert_eq!(format_pct(f64::NAN), "—");
    }

    #[test]
    fn format_currency_normal() {
        assert_eq!(format_currency(100000.0), "$100000");
    }

    #[test]
    fn format_currency_zero() {
        assert_eq!(format_currency(0.0), "—");
    }

    #[test]
    fn format_currency_nan() {
        assert_eq!(format_currency(f64::NAN), "—");
    }

    // ── Pure dispatch-planning logic (T5) ────────────────────────────────

    fn dispatchable_provenance() -> BlockProvenance {
        BlockProvenance {
            tool: Some("portfolio_returns".into()),
            server: Some("hkask-mcp-companies".into()),
            args: serde_json::json!({
                "portfolio": "main",
                "from": "2020-01-01",
                "to": "2024-12-31"
            }),
            span_id: None,
        }
    }

    #[test]
    fn build_args_dispatchable_merges_override_into_provenance_args()
    -> Result<(), Box<dyn std::error::Error>> {
        let provenance = dispatchable_provenance();
        let (server, tool, args) =
            build_returns_dispatch_args(&provenance, "2021-01-01", "2024-06-30")
                .map_err(String::from)?;
        assert_eq!(server, "hkask-mcp-companies");
        assert_eq!(tool, "portfolio_returns");
        assert_eq!(args["portfolio"], "main");
        assert_eq!(args["from"], "2021-01-01");
        assert_eq!(args["to"], "2024-06-30");
        Ok(())
    }

    #[test]
    fn build_args_empty_provenance_falls_back_to_default_server_and_tool()
    -> Result<(), Box<dyn std::error::Error>> {
        let provenance = BlockProvenance::default();
        let (server, tool, args) =
            build_returns_dispatch_args(&provenance, "2021-01-01", "2024-06-30")
                .map_err(String::from)?;
        assert_eq!(server, "hkask-mcp-companies");
        assert_eq!(tool, "portfolio_returns");
        assert_eq!(args["from"], "2021-01-01");
        assert_eq!(args["to"], "2024-06-30");
        assert!(
            args.get("portfolio").is_none(),
            "no portfolio name without provenance"
        );
        Ok(())
    }

    #[test]
    fn build_args_non_dispatchable_partial_provenance_is_disabled() {
        // tool present but server absent → not dispatchable, not empty → disabled.
        let provenance = BlockProvenance {
            tool: Some("portfolio_returns".into()),
            ..Default::default()
        };
        let result = build_returns_dispatch_args(&provenance, "2021-01-01", "2024-06-30");
        assert!(
            matches!(result, Err(PROVENANCE_INCOMPLETE_MSG)),
            "partial provenance is disabled"
        );
    }

    #[test]
    fn build_args_invalid_date_format_is_rejected() {
        let provenance = BlockProvenance::default();
        let result = build_returns_dispatch_args(&provenance, "not-a-date", "2024-06-30");
        assert!(
            matches!(result, Err(DATE_FORMAT_ERR)),
            "invalid from rejected"
        );
        let result = build_returns_dispatch_args(&provenance, "2021-01-01", "2024/06/30");
        assert!(
            matches!(result, Err(DATE_FORMAT_ERR)),
            "invalid to rejected"
        );
    }

    #[test]
    fn build_args_dispatchable_treats_null_args_as_empty_object()
    -> Result<(), Box<dyn std::error::Error>> {
        let provenance = BlockProvenance {
            tool: Some("portfolio_returns".into()),
            server: Some("hkask-mcp-companies".into()),
            args: serde_json::Value::Null,
            span_id: None,
        };
        let (server, tool, args) =
            build_returns_dispatch_args(&provenance, "2021-01-01", "2024-06-30")
                .map_err(String::from)?;
        assert_eq!(server, "hkask-mcp-companies");
        assert_eq!(tool, "portfolio_returns");
        assert_eq!(args["from"], "2021-01-01");
        assert_eq!(args["to"], "2024-06-30");
        Ok(())
    }

    #[test]
    fn is_valid_ymd_accepts_iso_dates_and_rejects_others() {
        assert!(is_valid_ymd("2024-01-01"));
        assert!(is_valid_ymd("1999-12-31"));
        assert!(!is_valid_ymd("2024-1-1"));
        assert!(!is_valid_ymd("2024/01/01"));
        assert!(!is_valid_ymd("not-a-date"));
        assert!(!is_valid_ymd(""));
        assert!(!is_valid_ymd("20240101"));
    }

    fn keystroke(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: key.to_string(),
            key_char: key_char.map(|character| character.to_string()),
        }
    }

    #[test]
    fn handle_date_keystroke_appends_digits_and_dash_and_handles_backspace() {
        let mut buffer = String::new();
        for typed in ["2", "0", "2", "4", "-", "0", "1"] {
            handle_date_keystroke(&mut buffer, &keystroke(typed, Some(typed)));
        }
        assert_eq!(buffer, "2024-01");

        handle_date_keystroke(&mut buffer, &keystroke("backspace", None));
        assert_eq!(buffer, "2024-0");

        // Non-digit/dash typed chars are ignored.
        handle_date_keystroke(&mut buffer, &keystroke("a", Some("a")));
        assert_eq!(buffer, "2024-0");

        // Buffer caps at 10 chars (YYYY-MM-DD).
        let mut full = "2024-01-01".to_string();
        handle_date_keystroke(&mut full, &keystroke("1", Some("1")));
        assert_eq!(full, "2024-01-01");
    }

    // ── GPUI integration tests via the governed `shared_tool_invoker()` ──────
    //
    // These mutate the process-global `ToolInvoker`, which is racy across
    // parallel tests (repo `.rules` racy-global trap). `GLOBAL_TEST_LOCK`
    // serializes the tests below within this crate's test binary so they never
    // observe each other's invoker.
    static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Records `(server, tool, args)` for every `invoke_tool` call and returns
    /// canned JSON. Implements `Send + Sync` for the `Arc<dyn ToolInvoker>` global.
    #[derive(Default)]
    struct MockToolInvoker {
        calls: std::sync::Mutex<Vec<(String, String, serde_json::Value)>>,
    }

    impl hkask_tool_invoker::ToolInvoker for MockToolInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> gpui::Task<Result<String, String>> {
            self.calls
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((server.to_string(), tool.to_string(), args));
            gpui::Task::ready(Ok("{}".to_string()))
        }
    }

    /// RAII guard that restores the global invoker to `None` on drop so a test
    /// failure cannot leak a mock into sibling tests.
    struct InvokerGuard;
    impl Drop for InvokerGuard {
        fn drop(&mut self) {
            hkask_tool_invoker::set_tool_invoker(None);
        }
    }

    fn body_with_provenance(provenance: BlockProvenance) -> PortfolioBlockBody {
        PortfolioBlockBody {
            viz: Some("portfolio".into()),
            portfolio: Some("main".into()),
            returns: None,
            characteristics: std::collections::HashMap::new(),
            attribution: Vec::new(),
            provenance,
        }
    }

    #[gpui::test]
    async fn dispatch_change_range_routes_through_invoker(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = InvokerGuard;
        let mock = std::sync::Arc::new(MockToolInvoker::default());
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        let body = body_with_provenance(dispatchable_provenance());
        let widget = cx.update(|cx| cx.new(|cx| PortfolioWidget::new(body, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.from_input = "2021-01-01".into();
                widget.to_input = "2024-06-30".into();
                widget.dispatch_change_range(cx);
            });
        });
        cx.run_until_parked();

        let calls = mock
            .calls
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(calls.len(), 1, "exactly one dispatch");
        assert_eq!(calls[0].0, "hkask-mcp-companies");
        assert_eq!(calls[0].1, "portfolio_returns");
        assert_eq!(calls[0].2["portfolio"], "main");
        assert_eq!(calls[0].2["from"], "2021-01-01");
        assert_eq!(calls[0].2["to"], "2024-06-30");

        let (in_flight, error) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_in_flight.clone(),
                    widget.dispatch_error.clone(),
                )
            })
        });
        assert!(in_flight.is_none(), "in-flight cleared after completion");
        assert!(error.is_none(), "no error on success");
    }

    #[gpui::test]
    async fn dispatch_change_range_surfaces_missing_invoker_as_visible_error(
        cx: &mut gpui::TestAppContext,
    ) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = InvokerGuard;
        hkask_tool_invoker::set_tool_invoker(None);

        // Empty provenance → fallback dispatch path, but the invoker is absent.
        let body = body_with_provenance(BlockProvenance::default());
        let widget = cx.update(|cx| cx.new(|cx| PortfolioWidget::new(body, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.from_input = "2021-01-01".into();
                widget.to_input = "2024-06-30".into();
                widget.dispatch_change_range(cx);
            });
        });
        cx.run_until_parked();

        let (error, in_flight) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_error.clone(),
                    widget.dispatch_in_flight.clone(),
                )
            })
        });
        assert_eq!(error.as_deref(), Some(INVOKER_NOT_WIRED_MSG));
        assert!(in_flight.is_none());
    }

    #[gpui::test]
    async fn dispatch_change_range_surfaces_non_dispatchable_provenance(
        cx: &mut gpui::TestAppContext,
    ) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _restore = InvokerGuard;
        // A mock is wired so the partial-provenance error is detected before
        // the invoker is consulted — no call should be recorded.
        let mock = std::sync::Arc::new(MockToolInvoker::default());
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        let partial = BlockProvenance {
            tool: Some("portfolio_returns".into()),
            ..Default::default()
        };
        let body = body_with_provenance(partial);
        let widget = cx.update(|cx| cx.new(|cx| PortfolioWidget::new(body, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| {
                widget.from_input = "2021-01-01".into();
                widget.to_input = "2024-06-30".into();
                widget.dispatch_change_range(cx);
            });
        });
        cx.run_until_parked();

        let (error, in_flight) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_error.clone(),
                    widget.dispatch_in_flight.clone(),
                )
            })
        });
        assert_eq!(error.as_deref(), Some(PROVENANCE_INCOMPLETE_MSG));
        assert!(in_flight.is_none());
        assert!(
            mock.calls
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty(),
            "no dispatch on non-dispatchable provenance"
        );
    }
}
