//! The `PortfolioWidget` GPUI view — renders the portfolio dashboard inline in
//! agent markdown (via the D18 seam composed by `hkask-viz-core`).
//!
//! This is a passive renderer: data comes from the parsed `PortfolioBlockBody`
//! (emitted by the curator agent after calling the `companies` MCP tools), not
//! from live `ToolInvoker` fetches. The portfolio selector, aggregation/date
//! controls, auto-loader, and comparison mode from the deleted standalone
//! `PortfolioDashboardView` are intentionally omitted — the agent picks the
//! portfolio and the block body already contains the data for it.
//!
//! Renders the same dashboard layout: header → summary tiles (total return,
//! IRR, modified Dietz, start/end value, positions) → returns detail →
//! characteristics table → attribution ranking. Read-only.

use gpui::{AnyElement, App, Context, FocusHandle, Focusable, Render, Window, prelude::*};
use ui::prelude::*;

use crate::block::{
    AttributionRow, CharacteristicField, FIBO_INTERNAL_RATE_OF_RETURN, FIBO_TIME_WEIGHTED_RETURN,
    FIBO_TRANSACTION_LEDGER, PortfolioBlockBody,
};

/// The portfolio widget view. Renders inline in agent markdown (via the D18
/// seam composed by `hkask-viz-core`).
pub struct PortfolioWidget {
    body: PortfolioBlockBody,
    focus_handle: FocusHandle,
}

impl PortfolioWidget {
    /// Create a new portfolio widget for the parsed block body.
    pub fn new(body: PortfolioBlockBody, cx: &mut Context<Self>) -> Self {
        Self {
            body,
            focus_handle: cx.focus_handle(),
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
        fields.sort_by(|(left, _), (right, _)| left.cmp(right));

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
}
