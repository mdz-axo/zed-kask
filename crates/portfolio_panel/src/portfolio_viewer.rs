//! Investor-oriented report viewer for the Portfolio panel's upper pane.
//!
//! Reports arrive as server-authored ```` ```portfolio ```` display hints on
//! completed tool calls. The viewer keeps the newest report of each kind and
//! never derives analytics from model-authored prose.

use std::collections::{BTreeMap, HashSet};

use acp_thread::AgentThreadEntry;
use agent_client_protocol::schema::v1::ToolCallId;
use gpui::{AnyElement, Context, Entity, Render, SharedString, Window};
use hkask_mcp_portfolio::{
    AttributionReport, CharacteristicsReport, ContributionReport, WhatIfReport,
};
use serde::Deserialize;
use serde_json::Value;
use ui::{Color, Label, LabelSize, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReportTab {
    Characteristics,
    Contribution,
    Attribution,
    WhatIf,
}

impl ReportTab {
    const ALL: [Self; 4] = [
        Self::Characteristics,
        Self::Contribution,
        Self::Attribution,
        Self::WhatIf,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Characteristics => "Characteristics",
            Self::Contribution => "Contribution",
            Self::Attribution => "Attribution",
            Self::WhatIf => "What-If",
        }
    }

    fn accepts(self, kind: &str) -> bool {
        match self {
            Self::Characteristics => kind == "characteristics",
            Self::Contribution => kind == "contribution",
            Self::Attribution => kind == "attribution",
            Self::WhatIf => matches!(kind, "what_if" | "historical_what_if"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct PortfolioReportBlock {
    viz: String,
    portfolio: String,
    report_kind: String,
    report: Value,
}

pub struct PortfolioViewer {
    active_tab: ReportTab,
    reports: BTreeMap<String, PortfolioReportBlock>,
    thread: Option<gpui::WeakEntity<acp_thread::AcpThread>>,
    processed_tool_results: HashSet<ToolCallId>,
}

impl PortfolioViewer {
    pub fn new() -> Self {
        Self {
            active_tab: ReportTab::Characteristics,
            reports: BTreeMap::new(),
            thread: None,
            processed_tool_results: HashSet::new(),
        }
    }

    pub fn ingest_thread(&mut self, thread: Entity<acp_thread::AcpThread>, cx: &mut Context<Self>) {
        if self.thread.as_ref().and_then(gpui::WeakEntity::upgrade) != Some(thread.clone()) {
            self.processed_tool_results.clear();
        }
        self.thread = Some(thread.downgrade());
        let mut completed = Vec::new();
        {
            let thread = thread.read(cx);
            for entry in thread.entries() {
                let AgentThreadEntry::ToolCall(call) = entry else {
                    continue;
                };
                let Some(output) = call.raw_output.as_ref() else {
                    continue;
                };
                if self.processed_tool_results.insert(call.id.clone()) {
                    completed.push(output.clone());
                }
            }
        }
        let mut changed = false;
        for output in completed {
            let hints = match &output {
                Value::String(text) => {
                    hkask_types::tool_response::display_hints_from_output_text(text)
                }
                value => hkask_types::tool_response::display_hints_from_output_value(value),
            };
            for hint in hints {
                let Some(report) = report_from_hint(&hint) else {
                    continue;
                };
                let tab = tab_for_kind(&report.report_kind);
                self.reports.insert(report.report_kind.clone(), report);
                if let Some(tab) = tab {
                    self.active_tab = tab;
                }
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn report_for_active_tab(&self) -> Option<&PortfolioReportBlock> {
        self.reports
            .values()
            .rev()
            .find(|report| self.active_tab.accepts(&report.report_kind))
    }

    fn render_tab(&self, tab: ReportTab, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active_tab == tab;
        div()
            .id(SharedString::from(format!(
                "portfolio-viewer-{}",
                tab.label()
            )))
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .when(active, |element| {
                element.bg(cx.theme().colors().element_selected)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_tab = tab;
                cx.notify();
            }))
            .child(
                Label::new(tab.label())
                    .size(LabelSize::Small)
                    .color(if active { Color::Default } else { Color::Muted }),
            )
            .into_any_element()
    }

    fn render_report(&self, report: &PortfolioReportBlock, cx: &mut Context<Self>) -> AnyElement {
        match report.report_kind.as_str() {
            "characteristics" => {
                serde_json::from_value::<CharacteristicsReport>(report.report.clone())
                    .map(|value| render_characteristics(&value, cx))
                    .unwrap_or_else(|error| {
                        render_error(&format!("Invalid characteristics report: {error}"))
                    })
            }
            "contribution" => serde_json::from_value::<ContributionReport>(report.report.clone())
                .map(|value| render_contribution(&value, cx))
                .unwrap_or_else(|error| {
                    render_error(&format!("Invalid contribution report: {error}"))
                }),
            "attribution" => serde_json::from_value::<AttributionReport>(report.report.clone())
                .map(|value| render_attribution(&value, cx))
                .unwrap_or_else(|error| {
                    render_error(&format!("Invalid attribution report: {error}"))
                }),
            "historical_what_if" => serde_json::from_value::<WhatIfReport>(report.report.clone())
                .map(|value| render_historical_what_if(&value, cx))
                .unwrap_or_else(|error| {
                    render_error(&format!("Invalid historical what-if report: {error}"))
                }),
            "what_if" => render_prospective_what_if(&report.report, cx),
            other => render_error(&format!("Unsupported portfolio report kind: {other}")),
        }
    }
}

impl Render for PortfolioViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let report = self.report_for_active_tab();
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .children(ReportTab::ALL.map(|tab| self.render_tab(tab, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .when_some(report, |element, report| {
                        element.child(self.render_report(report, cx))
                    })
                    .when(report.is_none(), |element| {
                        element.child(
                            v_flex()
                                .gap_1()
                                .child(Label::new(format!(
                                    "No {} report yet",
                                    self.active_tab.label().to_lowercase()
                                )))
                                .child(
                                    Label::new(
                                        "Use the steering workspace below to analyze a portfolio or open a prior analysis thread.",
                                    )
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                                ),
                        )
                    }),
            )
    }
}

fn report_from_hint(hint: &str) -> Option<PortfolioReportBlock> {
    let trimmed = hint.trim();
    let body = trimmed
        .strip_prefix("```portfolio")?
        .trim()
        .strip_suffix("```")?
        .trim();
    let report: PortfolioReportBlock = serde_json::from_str(body).ok()?;
    (report.viz == "portfolio").then_some(report)
}

fn tab_for_kind(kind: &str) -> Option<ReportTab> {
    ReportTab::ALL.into_iter().find(|tab| tab.accepts(kind))
}

fn render_characteristics(
    report: &CharacteristicsReport,
    cx: &mut Context<PortfolioViewer>,
) -> AnyElement {
    let mut cards = vec![
        metric_card(
            "Market value",
            format_currency(report.total_market_value),
            None,
            cx,
        ),
        metric_card("Cash weight", format_percent(report.cash_weight), None, cx),
        metric_card(
            "Top five weight",
            format_percent(report.top_five_weight),
            None,
            cx,
        ),
        metric_card(
            "Effective holdings",
            format!("{:.1}", report.effective_holdings),
            None,
            cx,
        ),
    ];
    cards.extend(report.metrics.iter().map(|(name, metric)| {
        metric_card(
            name,
            metric
                .value
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            Some(format!(
                "{} · {:.0}% weight coverage",
                metric.method,
                metric.weight_coverage * 100.0
            )),
            cx,
        )
    }));
    report_shell(
        &report.portfolio,
        &format!("Characteristics as of {}", report.date),
        cards,
        cx,
    )
}

fn render_contribution(
    report: &ContributionReport,
    cx: &mut Context<PortfolioViewer>,
) -> AnyElement {
    let rows = report.rows.iter().map(|row| {
        report_row(
            &row.symbol,
            &format!("{:+.0} bps", row.contribution_bps),
            &format!("{:+}", format_currency(row.profit)),
            cx,
        )
    });
    report_shell(
        &report.portfolio,
        &format!(
            "Contribution {} to {} · portfolio return {}",
            report.from,
            report.to,
            format_percent(report.portfolio_return)
        ),
        rows.collect(),
        cx,
    )
}

fn render_attribution(report: &AttributionReport, cx: &mut Context<PortfolioViewer>) -> AnyElement {
    let mut rows = vec![
        metric_card(
            "Allocation",
            format_percent(report.allocation_effect),
            None,
            cx,
        ),
        metric_card(
            "Selection",
            format_percent(report.selection_effect),
            None,
            cx,
        ),
        metric_card(
            "Interaction",
            format_percent(report.interaction_effect),
            None,
            cx,
        ),
    ];
    rows.extend(report.rows.iter().map(|row| {
        report_row(
            &row.group,
            &format!(
                "A {} · S {} · I {}",
                format_percent(row.allocation_effect),
                format_percent(row.selection_effect),
                format_percent(row.interaction_effect)
            ),
            &format!(
                "portfolio {} · benchmark {}",
                format_percent(row.portfolio_weight),
                format_percent(row.benchmark_weight)
            ),
            cx,
        )
    }));
    report_shell(
        &report.portfolio,
        &format!(
            "Attribution vs {} · active return {} · {}",
            report.benchmark,
            format_percent(report.active_return),
            report.model
        ),
        rows,
        cx,
    )
}

fn render_historical_what_if(
    report: &WhatIfReport,
    cx: &mut Context<PortfolioViewer>,
) -> AnyElement {
    report_shell(
        &report.portfolio,
        &format!("Historical what-if {} to {}", report.from, report.to),
        vec![
            metric_card(
                "Actual ending value",
                format_currency(report.actual_end_value),
                None,
                cx,
            ),
            metric_card(
                "Hypothetical ending value",
                format_currency(report.hypothetical_end_value),
                None,
                cx,
            ),
            metric_card(
                "Opportunity cost",
                format_currency(report.value_difference),
                Some(report.interpretation.clone()),
                cx,
            ),
            metric_card(
                "Return difference",
                format_percent(report.return_difference),
                None,
                cx,
            ),
        ],
        cx,
    )
}

fn render_prospective_what_if(report: &Value, cx: &mut Context<PortfolioViewer>) -> AnyElement {
    let actual = report
        .get("actual")
        .cloned()
        .and_then(|value| serde_json::from_value::<CharacteristicsReport>(value).ok());
    let hypothetical = report
        .get("hypothetical")
        .cloned()
        .and_then(|value| serde_json::from_value::<CharacteristicsReport>(value).ok());
    match (actual, hypothetical) {
        (Some(actual), Some(hypothetical)) => report_shell(
            &actual.portfolio,
            &format!("Prospective composition what-if as of {}", actual.date),
            vec![
                metric_card(
                    "Actual concentration",
                    format!("{:.3}", actual.concentration_hhi),
                    None,
                    cx,
                ),
                metric_card(
                    "Hypothetical concentration",
                    format!("{:.3}", hypothetical.concentration_hhi),
                    None,
                    cx,
                ),
                metric_card(
                    "Actual effective holdings",
                    format!("{:.1}", actual.effective_holdings),
                    None,
                    cx,
                ),
                metric_card(
                    "Hypothetical effective holdings",
                    format!("{:.1}", hypothetical.effective_holdings),
                    Some("Derived working view; authoritative ledger unchanged".to_string()),
                    cx,
                ),
            ],
            cx,
        ),
        _ => render_error("Invalid prospective what-if report"),
    }
}

fn report_shell(
    portfolio: &str,
    subtitle: &str,
    rows: Vec<AnyElement>,
    cx: &mut Context<PortfolioViewer>,
) -> AnyElement {
    v_flex()
        .gap_2()
        .child(Label::new(portfolio.to_string()).size(LabelSize::Large))
        .child(
            Label::new(subtitle.to_string())
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            v_flex()
                .gap_1()
                .children(rows)
                .border_t_1()
                .border_color(cx.theme().colors().border_variant)
                .pt_2(),
        )
        .into_any_element()
}

fn metric_card(
    label: impl Into<SharedString>,
    value: String,
    detail: Option<String>,
    cx: &mut Context<PortfolioViewer>,
) -> AnyElement {
    v_flex()
        .gap_0p5()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .child(
            Label::new(label)
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .child(Label::new(value).size(LabelSize::Small))
        .when_some(detail, |element, detail| {
            element.child(
                Label::new(detail)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
        })
        .into_any_element()
}

fn report_row(
    label: &str,
    primary: &str,
    secondary: &str,
    cx: &mut Context<PortfolioViewer>,
) -> AnyElement {
    h_flex()
        .justify_between()
        .gap_2()
        .p_2()
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(Label::new(label.to_string()).size(LabelSize::Small))
        .child(
            v_flex()
                .items_end()
                .child(Label::new(primary.to_string()).size(LabelSize::Small))
                .child(
                    Label::new(secondary.to_string())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                ),
        )
        .into_any_element()
}

fn render_error(message: &str) -> AnyElement {
    Label::new(message.to_string())
        .color(Color::Error)
        .into_any_element()
}

fn format_percent(value: f64) -> String {
    format!("{:+.2}%", value * 100.0)
}

fn format_currency(value: f64) -> String {
    format!("${value:,.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_hint_requires_portfolio_fence_and_discriminator() {
        let valid = report_from_hint(
            "```portfolio\n{\"viz\":\"portfolio\",\"portfolio\":\"main\",\"report_kind\":\"contribution\",\"report\":{}}\n```",
        )
        .expect("valid portfolio report");
        assert_eq!(valid.portfolio, "main");
        assert_eq!(valid.report_kind, "contribution");
        assert!(report_from_hint("```media\n{}\n```").is_none());
        assert!(report_from_hint("```portfolio\n{\"viz\":\"graph\"}\n```").is_none());
    }

    #[test]
    fn what_if_kinds_share_one_investor_tab() {
        assert!(ReportTab::WhatIf.accepts("what_if"));
        assert!(ReportTab::WhatIf.accepts("historical_what_if"));
        assert!(!ReportTab::WhatIf.accepts("contribution"));
    }
}
